//! Filesystem import adapter for the canonical library ingest queue.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use picto_library::{
    FolderId, ImmutableMediaFacts, Lifecycle, PreparedCollectionImport, PreparedImport,
    PreparedIngestJob, PreparedIngestPayload, Rating,
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::library_application::LibraryApplication;

const WATCH_STABLE_DELAY: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualImportInput {
    pub paths: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub source_urls: Vec<String>,
    pub lifecycle: Lifecycle,
    pub parent_folder_id: Option<FolderId>,
    #[serde(default)]
    pub preserve_structure: bool,
    #[serde(default = "default_true")]
    pub include_subfolders: bool,
    #[serde(default)]
    pub include_folders_without_media: bool,
    #[serde(default)]
    pub watch_source_folder: bool,
    #[serde(default)]
    pub delete_after_ingest: bool,
    #[serde(default)]
    pub group_files: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportEnqueueReport {
    pub discovered: usize,
    pub queued: usize,
    pub already_queued: usize,
    pub skipped: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderTreeAnalysisInput {
    pub path: String,
    pub destination_folder_id: Option<u32>,
    pub include_subfolders: bool,
    pub include_source_root: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct FolderTreeAnalysis {
    pub source_depth: usize,
    pub destination_depth: usize,
    pub retained_depth: usize,
    pub consolidated_levels: usize,
}

#[derive(Debug, Clone)]
struct ImportCandidate {
    path: PathBuf,
    relative_parent: Option<PathBuf>,
}

#[derive(Debug)]
struct WatchedFolder {
    folder_id: FolderId,
    path: PathBuf,
    recursive: bool,
}

#[derive(Debug)]
struct PendingWatch {
    folder_id: FolderId,
    path: PathBuf,
    relative_parent: Option<PathBuf>,
    metadata: fs::Metadata,
    job_key: String,
}

pub fn analyze_folder_tree(
    application: &LibraryApplication,
    input: &FolderTreeAnalysisInput,
) -> Result<FolderTreeAnalysis, String> {
    let path = fs::canonicalize(input.path.trim())
        .map_err(|error| format!("Failed to resolve folder: {error}"))?;
    if !path.is_dir() {
        return Err(format!("Selected path is not a folder: {}", path.display()));
    }
    let relative_depth = if input.include_subfolders {
        collect_structure_directories(&path, true)?
            .iter()
            .map(|relative| relative_folder_depth(relative).saturating_sub(1))
            .max()
            .unwrap_or(0)
    } else {
        0
    };
    let source_depth = relative_depth + usize::from(input.include_source_root);
    let destination = input.destination_folder_id.map(FolderId);
    let available = application
        .library()
        .folder_child_capacity(destination)
        .map_err(|error| error.to_string())?;
    let destination_depth = picto_library::MAX_FOLDER_DEPTH - available;
    let retained_depth = source_depth.min(available);
    Ok(FolderTreeAnalysis {
        source_depth,
        destination_depth,
        retained_depth,
        consolidated_levels: source_depth.saturating_sub(retained_depth),
    })
}

pub async fn enqueue_manual_import(
    application: &LibraryApplication,
    input: &ManualImportInput,
) -> Result<ImportEnqueueReport, String> {
    if input.paths.is_empty() {
        return Err("At least one import path is required".into());
    }
    if input.watch_source_folder {
        if !input.preserve_structure {
            return Err(
                "Watching an imported folder requires preserving its folder structure".into(),
            );
        }
        if input.paths.len() != 1 {
            return Err("Watching an imported folder requires exactly one source folder".into());
        }
    }
    let candidates = collect_manual_candidates(application.root(), input)?;
    let structure_directories = if input.preserve_structure && input.include_folders_without_media {
        let mut directories = Vec::new();
        for value in &input.paths {
            let path = fs::canonicalize(value)
                .map_err(|error| format!("Failed to resolve import path '{value}': {error}"))?;
            if path.is_dir() {
                directories.extend(collect_structure_directories(
                    &path,
                    input.include_subfolders,
                )?);
            }
        }
        directories
    } else {
        Vec::new()
    };
    let folder_capacity = input
        .preserve_structure
        .then(|| {
            application
                .library()
                .folder_child_capacity(input.parent_folder_id)
        })
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or(0);
    let invocation = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now();
    let mut folders = BTreeMap::new();
    let mut report = ImportEnqueueReport {
        discovered: candidates.len(),
        ..Default::default()
    };

    if input.preserve_structure && input.include_folders_without_media {
        for relative in &structure_directories {
            ensure_relative_folder(
                application,
                input.parent_folder_id,
                truncate_relative_folder(relative, folder_capacity).as_deref(),
                &mut folders,
            )?;
        }
    }

    let mut collection_members = Vec::new();
    let mut preparation_errors = Vec::new();
    let mut prepared_count = 0usize;
    let mut job_index = 0usize;
    for (index, candidate) in candidates.into_iter().enumerate() {
        let candidate_result = if is_zip(&candidate.path) {
            prepare_archive(
                application,
                &candidate.path,
                input,
                None,
                now.timestamp_millis(),
            )
            .await
        } else {
            prepare_import(
                &candidate.path,
                input,
                None,
                format!("manual:{invocation}:{index}"),
                now.timestamp_millis(),
            )
            .await
            .map(|value| vec![value])
        };
        match candidate_result {
            Ok(mut values) => {
                let folder_id = if input.preserve_structure {
                    ensure_relative_folder(
                        application,
                        input.parent_folder_id,
                        candidate
                            .relative_parent
                            .as_deref()
                            .and_then(|relative| {
                                truncate_relative_folder(relative, folder_capacity)
                            })
                            .as_deref(),
                        &mut folders,
                    )?
                } else {
                    input.parent_folder_id
                };
                for value in &mut values {
                    value.folders = folder_id.into_iter().collect();
                }
                prepared_count += values.len();
                let collect_as_collection =
                    input.group_files || report.discovered == 1 && values.len() > 1;
                if collect_as_collection {
                    collection_members.append(&mut values);
                } else {
                    for value in values {
                        let job = PreparedIngestJob {
                            job_key: format!("manual:{invocation}:{job_index}"),
                            source_kind: "manual".into(),
                            source_path: value.file_path.clone(),
                            source_item_id: None,
                            delete_after_ingest: input.delete_after_ingest,
                            payload: PreparedIngestPayload::Item(value),
                        };
                        enqueue(application, &job, &now.to_rfc3339(), &mut report)?;
                        job_index += 1;
                    }
                }
            }
            Err(error) => {
                report.skipped += 1;
                preparation_errors.push(error);
            }
        }
    }

    if prepared_count == 0 {
        if input.preserve_structure && input.include_folders_without_media {
            return Ok(report);
        }
        let detail = preparation_errors
            .first()
            .map(|error| format!(" First error: {error}"))
            .unwrap_or_default();
        return Err(format!(
            "No supported media files were found in the selected import.{}",
            detail
        ));
    }
    if !collection_members.is_empty() {
        let name = collection_name(&input.paths);
        let job = PreparedIngestJob {
            job_key: format!("manual:{invocation}:collection"),
            source_kind: "manual".into(),
            source_path: input.paths.join("\n"),
            source_item_id: None,
            delete_after_ingest: input.delete_after_ingest || report.discovered == 1,
            payload: PreparedIngestPayload::Collection(PreparedCollectionImport {
                members: collection_members,
                cover_index: 0,
                existing_root_id: None,
                name,
                modified_at_ms: now.timestamp_millis(),
            }),
        };
        enqueue(application, &job, &now.to_rfc3339(), &mut report)?;
    }
    if input.watch_source_folder {
        attach_source_folder_watch(application, input, &mut folders)?;
    }
    Ok(report)
}

fn attach_source_folder_watch(
    application: &LibraryApplication,
    input: &ManualImportInput,
    folders: &mut BTreeMap<(Option<u32>, String), FolderId>,
) -> Result<(), String> {
    let source = fs::canonicalize(&input.paths[0]).map_err(|error| {
        format!(
            "Failed to resolve watched import folder '{}': {error}",
            input.paths[0]
        )
    })?;
    if !source.is_dir() {
        return Err("Only a folder import can be watched".into());
    }
    let root_name = source
        .file_name()
        .map(PathBuf::from)
        .ok_or_else(|| "The watched import folder must have a name".to_owned())?;
    let capacity = application
        .library()
        .folder_child_capacity(input.parent_folder_id)
        .map_err(|error| error.to_string())?;
    let folder_id = ensure_relative_folder(
        application,
        input.parent_folder_id,
        truncate_relative_folder(&root_name, capacity).as_deref(),
        folders,
    )?
    .ok_or_else(|| "Could not create a destination for the watched folder".to_owned())?;
    application.set_folder_watch(&picto_library::FolderWatchInput {
        folder_id,
        path: source.to_string_lossy().into_owned(),
        include_subfolders: input.include_subfolders,
    })?;
    Ok(())
}

pub async fn scan_watched_folders(
    application: &LibraryApplication,
) -> Result<ImportEnqueueReport, String> {
    let enabled = application
        .application_settings()?
        .value
        .get("autoImportEnabled")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    if !enabled {
        return Ok(ImportEnqueueReport::default());
    }

    let watches = application
        .library()
        .auxiliary_read(
            picto_library::database::WorkPriority::CanonicalIngest,
            |connection| {
                let mut statement = connection.prepare(
                    "SELECT folder_id, watch_path, watch_subfolders
                 FROM folder_definition
                 WHERE watch_enabled = 1 AND watch_path IS NOT NULL
                 ORDER BY folder_id",
                )?;
                let rows = statement
                    .query_map([], |row| {
                        Ok(WatchedFolder {
                            folder_id: FolderId(row.get(0)?),
                            path: PathBuf::from(row.get::<_, String>(1)?),
                            recursive: row.get(2)?,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            },
        )
        .map_err(|error| error.to_string())?;
    if watches.is_empty() {
        return Ok(ImportEnqueueReport::default());
    }

    let mut existing = application
        .library()
        .auxiliary_read(
            picto_library::database::WorkPriority::CanonicalIngest,
            |connection| {
                let mut statement = connection.prepare(
                    "SELECT job_key FROM ingest_job
                 WHERE source_kind = 'watch' AND status <> 'failed'",
                )?;
                let keys = statement
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<HashSet<_>>>()?;
                Ok(keys)
            },
        )
        .map_err(|error| error.to_string())?;

    let (mut report, pending) =
        tokio::task::spawn_blocking(move || collect_watched_candidates(watches, &mut existing))
            .await
            .map_err(|error| format!("Watched-folder scan worker failed: {error}"))??;
    if !pending.is_empty() {
        tokio::time::sleep(WATCH_STABLE_DELAY).await;
    }

    let now = chrono::Utc::now();
    let input = ManualImportInput {
        paths: Vec::new(),
        tags: Vec::new(),
        source_urls: Vec::new(),
        lifecycle: Lifecycle::Inbox,
        parent_folder_id: None,
        preserve_structure: false,
        include_subfolders: true,
        include_folders_without_media: false,
        watch_source_folder: false,
        delete_after_ingest: false,
        group_files: false,
    };
    let mut folders = BTreeMap::new();
    for candidate in pending {
        if !file_is_stable(&candidate.path, &candidate.metadata) {
            report.skipped += 1;
            continue;
        }
        match prepare_import(
            &candidate.path,
            &input,
            None,
            candidate.job_key.clone(),
            now.timestamp_millis(),
        )
        .await
        {
            Ok(mut value) => {
                let capacity = application
                    .library()
                    .folder_child_capacity(Some(candidate.folder_id))
                    .map_err(|error| error.to_string())?;
                let folder_id = ensure_relative_folder(
                    application,
                    Some(candidate.folder_id),
                    candidate
                        .relative_parent
                        .as_deref()
                        .and_then(|relative| truncate_relative_folder(relative, capacity))
                        .as_deref(),
                    &mut folders,
                )?;
                value.folders = folder_id.into_iter().collect();
                enqueue(
                    application,
                    &PreparedIngestJob {
                        job_key: candidate.job_key,
                        source_kind: "watch".into(),
                        source_path: value.file_path.clone(),
                        source_item_id: None,
                        delete_after_ingest: false,
                        payload: PreparedIngestPayload::Item(value),
                    },
                    &now.to_rfc3339(),
                    &mut report,
                )?;
            }
            Err(error) if error.starts_with("Unsupported media:") => report.skipped += 1,
            Err(error) => return Err(error),
        }
    }
    Ok(report)
}

fn collect_watched_candidates(
    watches: Vec<WatchedFolder>,
    existing: &mut HashSet<String>,
) -> Result<(ImportEnqueueReport, Vec<PendingWatch>), String> {
    let mut report = ImportEnqueueReport::default();
    let mut pending = Vec::new();
    for watch in watches {
        for candidate in collect_directory(
            &watch.path,
            watch.recursive,
            watch.recursive.then(PathBuf::new),
        )? {
            report.discovered += 1;
            let metadata = match fs::metadata(&candidate.path) {
                Ok(metadata) => metadata,
                Err(_) => {
                    report.skipped += 1;
                    continue;
                }
            };
            let job_key = watch_job_key(watch.folder_id, &candidate.path, &metadata);
            if !existing.insert(job_key.clone()) {
                report.already_queued += 1;
                continue;
            }
            pending.push(PendingWatch {
                folder_id: watch.folder_id,
                path: candidate.path,
                relative_parent: candidate.relative_parent,
                metadata,
                job_key,
            });
        }
    }
    Ok((report, pending))
}

fn enqueue(
    application: &LibraryApplication,
    job: &PreparedIngestJob,
    now: &str,
    report: &mut ImportEnqueueReport,
) -> Result<(), String> {
    application
        .library()
        .enqueue_ingest_job(job, now)
        .map_err(|error| error.to_string())?;
    report.queued += 1;
    Ok(())
}

async fn prepare_archive(
    application: &LibraryApplication,
    archive_path: &Path,
    input: &ManualImportInput,
    folder_id: Option<FolderId>,
    imported_at_ms: i64,
) -> Result<Vec<PreparedImport>, String> {
    let staging = application.root().join("temp/archive-import");
    let entries =
        crate::media_processing::archive::extract_library_files(archive_path, &staging)
            .map_err(|error| format!("Failed to extract {}: {error}", archive_path.display()))?;
    if entries.is_empty() {
        return Err(format!(
            "Unsupported media: {} contains no accepted files",
            archive_path.display()
        ));
    }
    let mut members = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        match prepare_import(
            &entry.path,
            input,
            folder_id,
            format!("archive:{}:{index}", archive_path.display()),
            imported_at_ms,
        )
        .await
        {
            Ok(mut member) => {
                member.media_name = Path::new(&entry.archive_name)
                    .file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or(&member.media_name)
                    .to_owned();
                members.push(member);
            }
            Err(error) => {
                if let Some(directory) = entries.first().and_then(|entry| entry.path.parent()) {
                    let _ = fs::remove_dir_all(directory);
                }
                return Err(error);
            }
        }
    }
    Ok(members)
}

async fn prepare_import(
    path: &Path,
    input: &ManualImportInput,
    folder_id: Option<FolderId>,
    stable_key: String,
    imported_at_ms: i64,
) -> Result<PreparedImport, String> {
    let prepared = crate::media_processing::PreparedMediaSource::prepare_ingest(path)
        .await
        .map_err(|error| format!("Failed to inspect {}: {error}", path.display()))?;
    if !prepared.caps.ingest_supported || prepared.mime_type == "application/zip" {
        return Err(format!("Unsupported media: {}", path.display()));
    }
    let hash_path = path.to_path_buf();
    let content_hash = crate::media_processing::get_hash_from_path_background(hash_path.clone())
        .await
        .map(hex::encode)
        .map_err(|error| format!("Failed to hash {}: {error}", hash_path.display()))?;
    let size_bytes = fs::metadata(path)
        .map_err(|error| format!("Failed to inspect {}: {error}", path.display()))?
        .len();
    let captured_at_ms = fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.created().or_else(|_| metadata.modified()).ok())
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|value| i64::try_from(value.as_millis()).ok());
    Ok(PreparedImport {
        stable_key,
        media_name: path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("Untitled")
            .to_owned(),
        file_path: path.to_string_lossy().into_owned(),
        facts: ImmutableMediaFacts {
            mime: prepared.mime_type,
            size_bytes,
            width: prepared.pixel_width,
            height: prepared.pixel_height,
            duration_ms: prepared.duration_ms,
            frame_count: prepared.num_frames,
            content_hash,
            perceptual_hash: None,
            palette: Vec::new(),
        },
        lifecycle: input.lifecycle,
        rating: Rating::Unrated,
        notes: None,
        tags: input.tags.clone(),
        folders: folder_id.into_iter().collect(),
        source_urls: input.source_urls.clone(),
        source_identity: None,
        imported_at_ms,
        captured_at_ms,
    })
}

fn ensure_relative_folder(
    application: &LibraryApplication,
    parent: Option<FolderId>,
    relative: Option<&Path>,
    cache: &mut BTreeMap<(Option<u32>, String), FolderId>,
) -> Result<Option<FolderId>, String> {
    let Some(relative) = relative else {
        return Ok(parent);
    };
    let mut current = parent;
    for component in relative.components() {
        let name = component.as_os_str().to_string_lossy().trim().to_owned();
        if name.is_empty() {
            continue;
        }
        let key = (current.map(|id| id.0), name.clone());
        let folder_id = if let Some(folder_id) = cache.get(&key).copied() {
            folder_id
        } else if let Some(folder_id) = application
            .navigation()?
            .folders
            .into_iter()
            .find(|folder| folder.parent_id == current && folder.name == name)
            .map(|folder| folder.folder_id)
        {
            folder_id
        } else {
            application
                .create_folder(picto_library::CreateFolderInput {
                    name: name.clone(),
                    parent_id: current,
                })?
                .folder_id
        };
        cache.insert(key, folder_id);
        current = Some(folder_id);
    }
    Ok(current)
}

fn collect_manual_candidates(
    library_root: &Path,
    input: &ManualImportInput,
) -> Result<Vec<ImportCandidate>, String> {
    let library = fs::canonicalize(library_root)
        .map_err(|error| format!("Failed to resolve library path: {error}"))?;
    let mut candidates = Vec::new();
    for value in &input.paths {
        let path = fs::canonicalize(value)
            .map_err(|error| format!("Failed to resolve import path '{value}': {error}"))?;
        if path.starts_with(&library) {
            return Err(format!(
                "Import path must be outside the library: {}",
                path.display()
            ));
        }
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("Failed to inspect {}: {error}", path.display()))?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_file() {
            if is_media_path(&path) && !should_ignore(&path) {
                candidates.push(ImportCandidate {
                    path,
                    relative_parent: None,
                });
            }
        } else if metadata.is_dir() {
            let root_name = path.file_name().map(PathBuf::from);
            candidates.extend(collect_directory(
                &path,
                input.include_subfolders,
                input.preserve_structure.then_some(root_name).flatten(),
            )?);
        }
    }
    candidates.sort_by(|left, right| left.path.cmp(&right.path));
    candidates.dedup_by(|left, right| left.path == right.path);
    Ok(candidates)
}

fn collect_structure_directories(root: &Path, recursive: bool) -> Result<Vec<PathBuf>, String> {
    let root_name = root.file_name().map(PathBuf::from).unwrap_or_default();
    let mut directories = vec![root_name.clone()];
    if !recursive {
        return Ok(directories);
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in fs::read_dir(&directory)
            .map_err(|error| format!("Failed to read {}: {error}", directory.display()))?
        {
            let entry = entry.map_err(|error| format!("Failed to read directory: {error}"))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|error| format!("Failed to inspect {}: {error}", path.display()))?;
            if !should_ignore(&path) && file_type.is_dir() && !file_type.is_symlink() {
                if let Ok(relative) = path.strip_prefix(root) {
                    directories.push(root_name.join(relative));
                }
                stack.push(path);
            }
        }
    }
    directories.sort();
    directories.dedup();
    Ok(directories)
}

fn relative_folder_depth(path: &Path) -> usize {
    path.components()
        .filter(|component| matches!(component, std::path::Component::Normal(_)))
        .count()
}

fn truncate_relative_folder(path: &Path, maximum_depth: usize) -> Option<PathBuf> {
    let mut truncated = PathBuf::new();
    for component in path
        .components()
        .filter_map(|component| match component {
            std::path::Component::Normal(value) => Some(value),
            _ => None,
        })
        .take(maximum_depth)
    {
        truncated.push(component);
    }
    (!truncated.as_os_str().is_empty()).then_some(truncated)
}

fn collect_directory(
    root: &Path,
    recursive: bool,
    structure_root: Option<PathBuf>,
) -> Result<Vec<ImportCandidate>, String> {
    let mut files = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let mut entries = fs::read_dir(&directory)
            .map_err(|error| format!("Failed to read {}: {error}", directory.display()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Failed to read {}: {error}", directory.display()))?;
        entries.sort_by_key(|entry| entry.path());
        for entry in entries {
            let path = entry.path();
            if should_ignore(&path) {
                continue;
            }
            let file_type = entry
                .file_type()
                .map_err(|error| format!("Failed to inspect {}: {error}", path.display()))?;
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                if recursive {
                    stack.push(path);
                }
            } else if file_type.is_file() && is_media_path(&path) && !is_zip(&path) {
                let relative = path
                    .strip_prefix(root)
                    .ok()
                    .and_then(Path::parent)
                    .unwrap_or_else(|| Path::new(""))
                    .to_path_buf();
                files.push(ImportCandidate {
                    path,
                    relative_parent: structure_root.as_ref().map(|base| base.join(relative)),
                });
            }
        }
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

fn collection_name(paths: &[String]) -> Option<String> {
    let first = paths.first().map(Path::new)?;
    if paths.len() == 1 {
        return first
            .file_stem()
            .and_then(|value| value.to_str())
            .map(str::to_owned);
    }
    first
        .parent()
        .and_then(Path::file_name)
        .and_then(|value| value.to_str())
        .map(str::to_owned)
        .or_else(|| Some("Imported Collection".into()))
}

fn should_ignore(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    name.starts_with('.')
        || [".part", ".partial", ".crdownload", ".tmp", ".download"]
            .iter()
            .any(|suffix| name.ends_with(suffix))
}

fn is_media_path(path: &Path) -> bool {
    crate::media_processing::has_supported_extension(path)
}

fn is_zip(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
}

fn watch_job_key(folder_id: FolderId, path: &Path, metadata: &fs::Metadata) -> String {
    let modified = metadata
        .modified()
        .unwrap_or(SystemTime::UNIX_EPOCH)
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        "watch:{}:{}:{}:{}",
        folder_id.0,
        path.display(),
        metadata.len(),
        modified
    )
}

fn file_is_stable(path: &Path, previous: &fs::Metadata) -> bool {
    fs::metadata(path)
        .map(|current| {
            current.is_file()
                && current.len() == previous.len()
                && current.modified().ok() == previous.modified().ok()
        })
        .unwrap_or(false)
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use image::{Rgb, RgbImage};

    use super::*;

    fn png(path: &Path, color: [u8; 3]) {
        RgbImage::from_pixel(4, 4, Rgb(color)).save(path).unwrap();
    }

    fn input(paths: Vec<String>, group_files: bool) -> ManualImportInput {
        ManualImportInput {
            paths,
            tags: vec!["source:manual".into()],
            source_urls: vec!["https://example.test/manual".into()],
            lifecycle: Lifecycle::Inbox,
            parent_folder_id: None,
            preserve_structure: false,
            include_subfolders: true,
            include_folders_without_media: false,
            watch_source_folder: false,
            delete_after_ingest: false,
            group_files,
        }
    }

    #[tokio::test]
    async fn manual_import_queues_the_canonical_payload() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.png");
        png(&source, [10, 20, 30]);
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();

        let report = enqueue_manual_import(
            &application,
            &input(vec![source.to_string_lossy().into_owned()], false),
        )
        .await
        .unwrap();
        assert_eq!(report.discovered, 1);
        assert_eq!(report.queued, 1);

        let settled = crate::library_ingest_runtime::run_batch(&application, 64).unwrap();
        assert_eq!(settled.ingested, 1);
        let details = application.library().details(settled.root_ids[0]).unwrap();
        assert_eq!(details.lifecycle, Lifecycle::Inbox);
        assert_eq!(
            details.root.source_urls,
            vec!["https://example.test/manual"]
        );
        assert_eq!(details.tag_ids.len(), 1);
    }

    #[tokio::test]
    async fn manual_import_expands_zip_to_collection_and_cleans_staging() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first.png");
        let second = directory.path().join("second.png");
        png(&first, [10, 20, 30]);
        png(&second, [30, 20, 10]);
        let archive = directory.path().join("images.zip");
        let file = fs::File::create(&archive).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("nested/first.png", options).unwrap();
        std::io::copy(&mut fs::File::open(&first).unwrap(), &mut zip).unwrap();
        zip.start_file("second.png", options).unwrap();
        std::io::copy(&mut fs::File::open(&second).unwrap(), &mut zip).unwrap();
        zip.finish().unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();

        let report = enqueue_manual_import(
            &application,
            &input(vec![archive.to_string_lossy().into_owned()], false),
        )
        .await
        .unwrap();
        assert_eq!(report.queued, 1);

        let settled = crate::library_ingest_runtime::run_batch(&application, 64).unwrap();
        assert_eq!(settled.ingested, 1);
        let details = application.library().details(settled.root_ids[0]).unwrap();
        assert_eq!(details.root.kind, picto_library::RootKind::Collection);
        assert_eq!(details.media.len(), 2);
        assert!(archive.exists());
        let staging = application.root().join("temp/archive-import");
        assert!(fs::read_dir(staging).unwrap().next().is_none());
    }

    #[tokio::test]
    async fn folder_import_does_not_expand_zip_files() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("Downloads");
        fs::create_dir(&source).unwrap();
        let archive = source.join("images.zip");
        let mut zip = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
        zip.start_file("image.png", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"not needed because the archive must not be opened")
            .unwrap();
        zip.finish().unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();

        let mut folder_input = input(vec![source.to_string_lossy().into_owned()], false);
        folder_input.preserve_structure = true;
        let error = enqueue_manual_import(&application, &folder_input)
            .await
            .unwrap_err();

        assert!(error.contains("No supported media files"));
        assert!(!application.root().join("temp/archive-import").exists());
    }

    #[tokio::test]
    async fn grouped_manual_import_is_one_atomic_collection_job() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("first.png");
        let second = directory.path().join("second.png");
        png(&first, [10, 20, 30]);
        png(&second, [30, 20, 10]);
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();

        let report = enqueue_manual_import(
            &application,
            &input(
                vec![
                    first.to_string_lossy().into_owned(),
                    second.to_string_lossy().into_owned(),
                ],
                true,
            ),
        )
        .await
        .unwrap();
        assert_eq!(report.discovered, 2);
        assert_eq!(report.queued, 1);

        let settled = crate::library_ingest_runtime::run_batch(&application, 64).unwrap();
        assert_eq!(settled.ingested, 1);
        let details = application.library().details(settled.root_ids[0]).unwrap();
        assert_eq!(details.root.kind, picto_library::RootKind::Collection);
        assert_eq!(details.media.len(), 2);
        assert_eq!(details.tag_ids.len(), 1);
    }

    #[tokio::test]
    async fn folder_import_queues_valid_media_and_skips_a_later_invalid_file() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("Downloads");
        fs::create_dir(&source).unwrap();
        png(&source.join("a-good.png"), [10, 20, 30]);
        fs::write(source.join("z-empty.png"), []).unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let mut folder_input = input(vec![source.to_string_lossy().into_owned()], false);
        folder_input.preserve_structure = true;

        let report = enqueue_manual_import(&application, &folder_input)
            .await
            .unwrap();
        assert_eq!(report.discovered, 2);
        assert_eq!(report.queued, 1);
        assert_eq!(report.skipped, 1);

        let settled = crate::library_ingest_runtime::run_batch(&application, 64).unwrap();
        assert_eq!(settled.ingested, 1);
        let imported = application.library().details(settled.root_ids[0]).unwrap();
        let folder = application
            .navigation()
            .unwrap()
            .folders
            .into_iter()
            .find(|folder| folder.name == "Downloads")
            .expect("the successful file should create its preserved folder");
        assert_eq!(imported.folder_ids, vec![folder.folder_id]);
    }

    #[tokio::test]
    async fn folder_import_can_watch_its_preserved_root_for_future_media() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("Photos");
        fs::create_dir(&source).unwrap();
        png(&source.join("first.png"), [10, 20, 30]);
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let mut folder_input = input(vec![source.to_string_lossy().into_owned()], false);
        folder_input.preserve_structure = true;
        folder_input.watch_source_folder = true;
        folder_input.include_subfolders = false;

        enqueue_manual_import(&application, &folder_input)
            .await
            .unwrap();

        let folder = application
            .navigation()
            .unwrap()
            .folders
            .into_iter()
            .find(|folder| folder.name == "Photos")
            .expect("the imported root folder should exist");
        assert!(folder.watch_enabled);
        assert_eq!(
            folder.watch_path.as_deref(),
            Some(
                fs::canonicalize(&source)
                    .unwrap()
                    .to_string_lossy()
                    .as_ref()
            )
        );
        assert!(!folder.watch_subfolders);
    }

    #[tokio::test]
    async fn invalid_folder_import_reports_an_error_without_creating_structure() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("Downloads");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("empty.png"), []).unwrap();
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let mut folder_input = input(vec![source.to_string_lossy().into_owned()], false);
        folder_input.preserve_structure = true;

        let error = enqueue_manual_import(&application, &folder_input)
            .await
            .unwrap_err();
        assert!(error.contains("No supported media files"));
        assert!(application.navigation().unwrap().folders.is_empty());
    }

    #[tokio::test]
    async fn folder_import_consolidates_a_tree_deeper_than_eight_without_skipping_media() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("Downloads");
        let mut deepest = source.clone();
        for depth in 2..=9 {
            deepest = deepest.join(format!("level-{depth}"));
        }
        fs::create_dir_all(&deepest).unwrap();
        png(&deepest.join("source.png"), [10, 20, 30]);
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let mut folder_input = input(vec![source.to_string_lossy().into_owned()], false);
        folder_input.preserve_structure = true;

        let analysis = analyze_folder_tree(
            &application,
            &FolderTreeAnalysisInput {
                path: source.to_string_lossy().into_owned(),
                destination_folder_id: None,
                include_subfolders: true,
                include_source_root: true,
            },
        )
        .unwrap();
        assert_eq!(analysis.source_depth, 9);
        assert_eq!(analysis.retained_depth, 8);
        assert_eq!(analysis.consolidated_levels, 1);

        let report = enqueue_manual_import(&application, &folder_input)
            .await
            .unwrap();
        assert_eq!(report.queued, 1);
        let folders = application.navigation().unwrap().folders;
        assert_eq!(folders.len(), 8);
        assert!(!folders.iter().any(|folder| folder.name == "level-9"));
        let retained = folders
            .iter()
            .find(|folder| folder.name == "level-8")
            .unwrap()
            .folder_id;
        let settled = crate::library_ingest_runtime::run_batch(&application, 64).unwrap();
        let details = application.library().details(settled.root_ids[0]).unwrap();
        assert_eq!(details.folder_ids, vec![retained]);
    }

    #[tokio::test]
    async fn watched_folder_queues_each_stable_file_once_for_inbox() {
        let directory = tempfile::tempdir().unwrap();
        let watched = directory.path().join("watched");
        fs::create_dir(&watched).unwrap();
        let source = watched.join("source.png");
        png(&source, [10, 20, 30]);
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();
        let folder = application
            .create_folder(picto_library::CreateFolderInput {
                name: "Watched".into(),
                parent_id: None,
            })
            .unwrap();
        application
            .set_folder_watch(&picto_library::FolderWatchInput {
                folder_id: folder.folder_id,
                path: watched.to_string_lossy().into_owned(),
                include_subfolders: true,
            })
            .unwrap();

        let first = scan_watched_folders(&application).await.unwrap();
        let second = scan_watched_folders(&application).await.unwrap();
        assert_eq!(first.queued, 1);
        assert_eq!(second.already_queued, 1);

        let settled = crate::library_ingest_runtime::run_batch(&application, 64).unwrap();
        let details = application.library().details(settled.root_ids[0]).unwrap();
        assert_eq!(details.lifecycle, Lifecycle::Inbox);
        assert_eq!(details.folder_ids, vec![folder.folder_id]);
    }

    #[tokio::test]
    async fn watched_folder_consolidates_paths_below_the_remaining_depth() {
        let directory = tempfile::tempdir().unwrap();
        let watched = directory.path().join("watched");
        let source_parent = watched.join("first").join("second").join("third");
        fs::create_dir_all(&source_parent).unwrap();
        png(&source_parent.join("source.png"), [10, 20, 30]);
        let application = LibraryApplication::create(directory.path().join("library")).unwrap();

        let mut destination = None;
        for depth in 1..=7 {
            destination = Some(
                application
                    .create_folder(picto_library::CreateFolderInput {
                        name: format!("Destination {depth}"),
                        parent_id: destination,
                    })
                    .unwrap()
                    .folder_id,
            );
        }
        let destination = destination.unwrap();
        application
            .set_folder_watch(&picto_library::FolderWatchInput {
                folder_id: destination,
                path: watched.to_string_lossy().into_owned(),
                include_subfolders: true,
            })
            .unwrap();

        let analysis = analyze_folder_tree(
            &application,
            &FolderTreeAnalysisInput {
                path: watched.to_string_lossy().into_owned(),
                destination_folder_id: Some(destination.0),
                include_subfolders: true,
                include_source_root: false,
            },
        )
        .unwrap();
        assert_eq!(analysis.retained_depth, 1);
        assert_eq!(analysis.consolidated_levels, 2);

        let report = scan_watched_folders(&application).await.unwrap();
        assert_eq!(report.queued, 1);
        let folders = application.navigation().unwrap().folders;
        let retained = folders
            .iter()
            .find(|folder| folder.parent_id == Some(destination) && folder.name == "first")
            .unwrap()
            .folder_id;
        assert!(!folders.iter().any(|folder| folder.name == "second"));
        let settled = crate::library_ingest_runtime::run_batch(&application, 64).unwrap();
        let details = application.library().details(settled.root_ids[0]).unwrap();
        assert_eq!(details.folder_ids, vec![retained]);
    }
}
